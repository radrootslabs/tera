//! Scoped operation state stays separate from the still-editable composer.

use radroots_storage::{
    Error,
    authored_draft::{AuthoredDraft, AuthoredDraftStage},
};
use radroots_sync::PushStatus;

use super::{
    SubmissionCommitError, SubmissionReceipt, SubmissionReservationRequest, intent,
    operation_load::LoadedOperation, repository::SubmissionRepository,
};
use crate::{
    TeraRuntime,
    runtime::product_surface::{
        Phase1DraftError, Phase1OutboxState, outbox, phase1_operation_now_unix_ms,
    },
};

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum SubmissionOperationError {
    #[error(transparent)]
    Submission(#[from] SubmissionCommitError),
    #[error(transparent)]
    Operation(#[from] Phase1DraftError),
    #[error("submission operation was not committed")]
    NotFound,
    #[error("submission prerequisites are incomplete")]
    PrerequisitesPending,
    #[error("submission operation requires repair")]
    Corrupt,
}

impl From<Error> for SubmissionOperationError {
    fn from(error: Error) -> Self {
        Self::Submission(error.into())
    }
}

#[derive(Clone, Eq, PartialEq)]
pub struct SubmissionOperationStatus {
    receipt: SubmissionReceipt,
    intent: AuthoredDraft,
    push: PushStatus,
}

impl std::fmt::Debug for SubmissionOperationStatus {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SubmissionOperationStatus")
            .field("state", &self.state())
            .finish_non_exhaustive()
    }
}

impl SubmissionOperationStatus {
    pub fn receipt(&self) -> &SubmissionReceipt {
        &self.receipt
    }
    pub fn intent(&self) -> &AuthoredDraft {
        &self.intent
    }
    pub fn push(&self) -> &PushStatus {
        &self.push
    }
    pub fn state(&self) -> Phase1OutboxState {
        let push = (self.intent.stage() == AuthoredDraftStage::Queued).then_some(&self.push);
        outbox::aggregate_state(&self.intent, push)
    }
}

type E = SubmissionOperationError;

impl TeraRuntime {
    pub async fn submission_operation_status(
        &self,
        request: &SubmissionReservationRequest,
    ) -> Result<SubmissionOperationStatus, E> {
        let _command = self.lifecycle.enter().map_err(Phase1DraftError::from)?;
        let (loaded, push) = self.load_submission_operation(request).await?;
        Ok(SubmissionOperationStatus {
            receipt: loaded.receipt,
            intent: loaded.head,
            push,
        })
    }

    /// Associates only the already committed ready operation; invokes no signer or transport.
    pub async fn submission_queue(
        &self,
        request: &SubmissionReservationRequest,
        expected_revision: u64,
    ) -> Result<SubmissionOperationStatus, E> {
        let _command = self.lifecycle.enter().map_err(Phase1DraftError::from)?;
        self.validate_submission_owner(request)?;
        let _admission = self
            .mutations
            .draft(*intent::intent_id(request)?.as_bytes())?;
        let (mut loaded, _) = self.load_submission_operation(request).await?;
        self.queue_submission_loaded(&mut loaded, expected_revision)
            .await?;
        self.submission_operation_status(request).await
    }

    /// Advances the captured operation through at most one existing bounded delivery attempt.
    pub async fn submission_advance(
        &self,
        request: &SubmissionReservationRequest,
        expected_revision: u64,
    ) -> Result<SubmissionOperationStatus, E> {
        let _command = self.lifecycle.enter().map_err(Phase1DraftError::from)?;
        self.validate_submission_owner(request)?;
        let _admission = self
            .mutations
            .draft(*intent::intent_id(request)?.as_bytes())?;
        let (mut loaded, _) = self.load_submission_operation(request).await?;
        self.queue_submission_loaded(&mut loaded, expected_revision)
            .await?;
        self.advance_push_request(loaded.request).await?;
        self.submission_operation_status(request).await
    }

    async fn load_submission_operation(
        &self,
        request: &SubmissionReservationRequest,
    ) -> Result<(LoadedOperation, PushStatus), E> {
        self.validate_submission_owner(request)?;
        let store = self
            .client
            .storage()
            .map_err(|_| Error::BackendUnavailable)?;
        let loaded = SubmissionRepository { store }
            .load_operation(request)
            .await?;
        let push = self
            .sync()?
            .push_status(loaded.request.operation_id())
            .await
            .map_err(|_| Phase1DraftError::Operation)?
            .ok_or(E::Corrupt)?;
        loaded.validate_push(&push)?;
        Ok((loaded, push))
    }

    async fn queue_submission_loaded(
        &self,
        loaded: &mut LoadedOperation,
        expected_revision: u64,
    ) -> Result<(), E> {
        if loaded.head.revision().get() != expected_revision {
            return Err(Phase1DraftError::RevisionConflict.into());
        }
        match loaded.head.stage() {
            AuthoredDraftStage::Queued => return Ok(()),
            AuthoredDraftStage::ReadyToSign => {}
            _ => return Err(E::PrerequisitesPending),
        }
        let next = loaded.head.successor(
            loaded.head.payload().to_vec(),
            AuthoredDraftStage::Queued,
            Some(loaded.receipt.operation_id()),
            phase1_operation_now_unix_ms()?.max(loaded.head.updated_at_unix_ms()),
        )?;
        let store = self
            .client
            .storage()
            .map_err(|_| Error::BackendUnavailable)?;
        let receipt = store
            .append_authored_draft(next.clone(), Some(loaded.head.revision()))
            .await?;
        if receipt.draft() != &next {
            return Err(E::Corrupt);
        }
        loaded.head = next;
        Ok(())
    }
}
