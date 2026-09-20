use super::*;
use crate::{
    TeraRuntime,
    runtime::product_surface::{
        Phase1DraftError, phase1_operation_now_unix_ms,
        recovery_completion::{
            RecoveryCompletionReceipt, RecoveryMedia, RecoveryNativeReceipt, admit,
            validate_history,
        },
        recovery_reconciliation::RecoveryDecision,
    },
};

impl TeraRuntime {
    pub(in crate::runtime::product_surface) async fn recover_submission_upload(
        &self,
        request: &SubmissionReservationRequest,
        native: RecoveryNativeReceipt,
        source: RecoveryMedia,
    ) -> Result<RecoveryCompletionReceipt, SubmissionOperationError> {
        self.validate_submission_owner(request)?;
        let id = intent::intent_id(request)?;
        if id != native.identity.parent {
            return Err(SubmissionOperationError::InvalidMedia);
        }
        let _admission = self.mutations.draft(*id.as_bytes())?;
        let (mut loaded, _) = self.load_submission_operation(request).await?;
        let store = self
            .client
            .storage()
            .map_err(|_| Error::BackendUnavailable)?;
        let historical = store
            .authored_draft_revision(id, native.identity.revision)
            .await?
            .ok_or(SubmissionOperationError::InvalidMedia)?;
        validate_history(&loaded.head, &historical, &native)?;
        let mut prior = loaded
            .payload
            .current(&historical, loaded.receipt.operation_id())?;
        let old = prior.media_mut(&source.reference)?;
        let media = loaded.payload.media_mut(&source.reference)?.clone();
        let captured = loaded
            .captured
            .form()
            .input()
            .media
            .iter()
            .find(|value| value.opaque_reference == source.reference)
            .ok_or(SubmissionOperationError::InvalidMedia)?;
        let dimensions =
            radroots_sdk::transport::BlossomImageDimensions::new(captured.width, captured.height)
                .map_err(|_| SubmissionOperationError::InvalidMedia)?;
        if dimensions != source.dimensions {
            return Err(SubmissionOperationError::InvalidMedia);
        }
        let author = radroots_identity::PublicKey::from_bytes(*loaded.head.author())
            .map_err(|_| SubmissionOperationError::Corrupt)?;
        if admit(author, &media, old, &native, &source)? == RecoveryDecision::Settle {
            return Ok(RecoveryCompletionReceipt::durable(&native, &media)?);
        }
        if loaded.head.operation_id().is_some() {
            return Err(SubmissionOperationError::InvalidMedia);
        }
        let blossom = self
            .client
            .blossom()
            .map_err(|_| Phase1DraftError::OperationUnavailable)?
            .ok_or(Phase1DraftError::OperationUnavailable)?;
        let transaction = blossom
            .prepare_upload(source.request_at(phase1_operation_now_unix_ms()?)?)
            .map_err(|_| SubmissionOperationError::InvalidMedia)?;
        if Some(*transaction.config_fingerprint().as_bytes()) != loaded.payload.media_policy() {
            return Err(SubmissionOperationError::MediaPolicyChanged);
        }
        if transaction.expected_url().as_str() != media.url() {
            return Err(SubmissionOperationError::InvalidMedia);
        }
        let (status, kind, encoding, body) = native.response.parts();
        let receipt = blossom
            .complete_native_upload(
                transaction,
                status,
                kind,
                encoding,
                body,
                radroots_sdk::transport::BlossomCancellation::default(),
            )
            .await
            .map_err(|_| Phase1DraftError::Operation)?;
        loaded
            .payload
            .media_mut(&source.reference)?
            .complete_recovered_transfer(&receipt)?;
        self.save_submission_media(&mut loaded).await?;
        Ok(RecoveryCompletionReceipt::durable(
            &native,
            loaded.payload.media_mut(&source.reference)?,
        )?)
    }
}
