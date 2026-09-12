//! One existing storage owner commits the full intent and source association.

use std::sync::Arc;

use radroots_storage::{
    Error,
    atomic::AtomicCommitDisposition,
    authored_atomic::{
        AuthoredAtomicCommand, AuthoredAtomicOutcome, AuthoredAtomicReceipt, AuthoredAtomicStorage,
    },
    authored_draft::{AuthoredDraftId, AuthoredDraftStore},
    journal::OperationInstanceId,
};

use super::{
    CapturedSubmission, SubmissionCaptureError, SubmissionReservationError,
    SubmissionReservationRequest, intent, repository::SubmissionRepository,
};
use crate::{TeraRuntime, runtime::product_surface::ComposerPersistenceError};

#[cfg(test)]
#[path = "commit_sqlite_tests.rs"]
mod sqlite_tests;
#[cfg(test)]
#[path = "commit_tests.rs"]
mod tests;

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum SubmissionCommitError {
    #[error(transparent)]
    Capture(SubmissionCaptureError),
    #[error(transparent)]
    Reservation(SubmissionReservationError),
    #[error("submission command conflicts with its committed request")]
    IdempotencyConflict,
    #[error("source composer revision changed before submission")]
    RevisionConflict,
    #[error("submission intent is invalid or exceeds its bounds")]
    InvalidIntent,
    #[error("submission record requires repair")]
    CorruptRecord,
    #[error("submission record schema is unsupported")]
    UnsupportedSchema,
    #[error("submission receipt is unconfirmed")]
    InvalidReceipt,
    #[error("submission storage failed: {0}")]
    Storage(Error),
}

impl From<Error> for SubmissionCommitError {
    fn from(error: Error) -> Self {
        match error {
            Error::AtomicCommitConflict => Self::IdempotencyConflict,
            Error::DraftRevisionConflict => Self::RevisionConflict,
            error => Self::Storage(error),
        }
    }
}

impl From<SubmissionReservationError> for SubmissionCommitError {
    fn from(error: SubmissionReservationError) -> Self {
        match error {
            SubmissionReservationError::IdempotencyConflict => Self::IdempotencyConflict,
            SubmissionReservationError::Source(ComposerPersistenceError::RevisionConflict) => {
                Self::RevisionConflict
            }
            error => Self::Reservation(error),
        }
    }
}

impl From<SubmissionCaptureError> for SubmissionCommitError {
    fn from(error: SubmissionCaptureError) -> Self {
        match error {
            SubmissionCaptureError::Reservation(error) => error.into(),
            error => Self::Capture(error),
        }
    }
}

/// A durable local operation handle; it is not a signing or delivery receipt.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SubmissionReceipt {
    request: SubmissionReservationRequest,
    intent_id: AuthoredDraftId,
    operation_id: OperationInstanceId,
    captured_at_unix_ms: u64,
    committed_at_unix_ms: u64,
    replayed: bool,
}

impl SubmissionReceipt {
    pub fn request(&self) -> &SubmissionReservationRequest {
        &self.request
    }
    pub const fn intent_id(&self) -> AuthoredDraftId {
        self.intent_id
    }
    pub const fn operation_id(&self) -> OperationInstanceId {
        self.operation_id
    }
    pub const fn captured_at_unix_ms(&self) -> u64 {
        self.captured_at_unix_ms
    }
    pub const fn committed_at_unix_ms(&self) -> u64 {
        self.committed_at_unix_ms
    }
    pub const fn is_replay(&self) -> bool {
        self.replayed
    }
}

type E = SubmissionCommitError;

impl<S: AuthoredDraftStore + AuthoredAtomicStorage + ?Sized> SubmissionRepository<'_, S> {
    pub(super) async fn commit(
        &self,
        captured: &CapturedSubmission,
    ) -> Result<SubmissionReceipt, E> {
        let reservation = self
            .replay(captured.reservation().request())
            .await?
            .ok_or(E::InvalidReceipt)?;
        if !reservation.same_request(captured.reservation()) {
            return Err(E::IdempotencyConflict);
        }
        let request = intent::IntentPayload::capture(captured)?;
        let command = AuthoredAtomicCommand::PrepareFromDraft(Box::new(request));
        // The owner performs full replay comparison before the original source CAS.
        // No prompt, media operation or network await enters this transaction.
        let receipt = self.store.execute_authored(command.clone()).await?;
        if !receipt.matches_command(&command) {
            return Err(E::InvalidReceipt);
        }
        self.committed_receipt(captured.reservation().request(), receipt, false)
            .await
    }

    pub(super) async fn recover(
        &self,
        request: &SubmissionReservationRequest,
    ) -> Result<Option<SubmissionReceipt>, E> {
        let Some(receipt) = self
            .store
            .authored_receipt(intent::commit_id(request))
            .await?
        else {
            return Ok(None);
        };
        self.committed_receipt(request, receipt, true)
            .await
            .map(Some)
    }

    async fn committed_receipt(
        &self,
        request: &SubmissionReservationRequest,
        receipt: AuthoredAtomicReceipt,
        lookup: bool,
    ) -> Result<SubmissionReceipt, E> {
        if receipt.commit_id() != intent::commit_id(request) {
            return Err(E::InvalidReceipt);
        }
        let AuthoredAtomicOutcome::Submitted(committed) = receipt.outcome() else {
            return Err(E::InvalidReceipt);
        };
        let reservation = self.replay(request).await?.ok_or(E::InvalidReceipt)?;
        intent::IntentPayload::validate_committed(committed, &reservation)?;
        if !receipt.matches_command(&AuthoredAtomicCommand::PrepareFromDraft(committed.clone())) {
            return Err(E::InvalidReceipt);
        }
        Ok(SubmissionReceipt {
            request: request.clone(),
            intent_id: committed.intent().draft_id(),
            operation_id: committed.preparation().operation().operation_id(),
            captured_at_unix_ms: reservation.reserved_at_unix_ms(),
            committed_at_unix_ms: receipt.committed_at_unix_ms(),
            replayed: lookup || receipt.disposition() == AtomicCommitDisposition::Replay,
        })
    }
}

impl TeraRuntime {
    /// Commits the exact owned capture. Changed capture/policy with the same ID conflicts.
    pub async fn submission_commit(
        &self,
        captured: &CapturedSubmission,
    ) -> Result<SubmissionReceipt, E> {
        let _command = self
            .lifecycle
            .enter()
            .map_err(SubmissionReservationError::from)?;
        self.validate_submission_owner(captured.reservation().request())?;
        let store = self
            .client
            .storage()
            .map_err(|_| Error::BackendUnavailable)?;
        SubmissionRepository { store }.commit(captured).await
    }

    /// Recovers committed work without consulting current settings or requiring local media.
    pub async fn submission_recover(
        &self,
        request: &SubmissionReservationRequest,
    ) -> Result<Option<SubmissionReceipt>, E> {
        let _command = self
            .lifecycle
            .enter()
            .map_err(SubmissionReservationError::from)?;
        self.validate_submission_owner(request)?;
        let store = self
            .client
            .storage()
            .map_err(|_| Error::BackendUnavailable)?;
        SubmissionRepository { store }.recover(request).await
    }

    /// Resolves the immutable saved request, then commits it before any publication effect.
    /// Bytes materialize the source's fixed media metadata only when capture is still needed.
    pub async fn submission_prepare(
        &self,
        request: &SubmissionReservationRequest,
        media_bytes: Vec<Arc<[u8]>>,
    ) -> Result<SubmissionReceipt, E> {
        let _command = self
            .lifecycle
            .enter()
            .map_err(SubmissionReservationError::from)?;
        if let Some(receipt) = self.submission_recover(request).await? {
            return Ok(receipt);
        }
        let captured = self.submission_capture(request, media_bytes).await?;
        self.submission_commit(&captured).await
    }

    fn validate_submission_owner(&self, request: &SubmissionReservationRequest) -> Result<(), E> {
        let author = self
            .store_public_key
            .ok_or(SubmissionReservationError::Source(
                ComposerPersistenceError::OwnerUnavailable,
            ))?;
        if author != request.scope().author() {
            return Err(SubmissionReservationError::Source(
                ComposerPersistenceError::ScopeMismatch,
            )
            .into());
        }
        Ok(())
    }
}
