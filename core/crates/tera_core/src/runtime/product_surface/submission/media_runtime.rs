use radroots_storage::{Error, authored_draft::AuthoredDraftStage};

use super::*;
use crate::runtime::product_surface::submission::SubmissionOperationStatus;
use crate::runtime::product_surface::{
    Phase1NativeUploadJob, Phase1UploadPlan, phase1_new_operation_id, phase1_operation_now_unix_ms,
};

impl TeraRuntime {
    /// Validates the committed parent before authorizing one immutable native upload job.
    pub async fn submission_prepare_native_upload(
        &self,
        input: SubmissionMediaRequest,
    ) -> Result<(SubmissionOperationStatus, Phase1NativeUploadJob), E> {
        let _command = self.lifecycle.enter().map_err(Phase1DraftError::from)?;
        self.validate_submission_owner(&input.submission)?;
        let id = super::super::intent::intent_id(&input.submission)?;
        let _admission = self.mutations.draft(*id.as_bytes())?;
        let (mut loaded, _) = self.load_submission_operation(&input.submission).await?;
        let transaction = self.materialize_submission_media(&loaded, &input, false)?;
        // Validate the exact transition before invoking a potentially interactive signer.
        loaded
            .payload
            .media_mut(&input.reference)?
            .transition_requested(Phase1MediaStage::Uploading, None)?;
        let plan = Phase1UploadPlan::derive(
            phase1_operation_now_unix_ms()?,
            phase1_new_operation_id()?,
            phase1_new_operation_id()?,
        )?;
        let job = self.authorize_native_upload(&transaction, &plan).await?;
        let blossom = self
            .client
            .blossom()
            .map_err(|_| Phase1DraftError::OperationUnavailable)?
            .ok_or(Phase1DraftError::OperationUnavailable)?;
        if blossom.config_fingerprint() != Some(transaction.config_fingerprint()) {
            return Err(E::MediaPolicyChanged);
        }
        self.save_submission_media(&mut loaded).await?;
        Ok((
            self.submission_operation_status(&input.submission).await?,
            job,
        ))
    }

    /// Verifies native HTTP evidence and canonical remote bytes through the existing SDK.
    pub async fn submission_complete_native_upload(
        &self,
        input: SubmissionMediaRequest,
        response: SubmissionMediaResponse,
    ) -> Result<SubmissionOperationStatus, E> {
        let _command = self.lifecycle.enter().map_err(Phase1DraftError::from)?;
        self.validate_submission_owner(&input.submission)?;
        let id = super::super::intent::intent_id(&input.submission)?;
        let _admission = self.mutations.draft(*id.as_bytes())?;
        let (mut loaded, _) = self.load_submission_operation(&input.submission).await?;
        let transaction = self.materialize_submission_media(&loaded, &input, true)?;
        let blossom = self
            .client
            .blossom()
            .map_err(|_| Phase1DraftError::OperationUnavailable)?
            .ok_or(Phase1DraftError::OperationUnavailable)?;
        match blossom
            .complete_native_upload(
                transaction,
                response.status_code,
                response.media_type.as_deref(),
                response.content_encoding.as_deref(),
                &response.body,
                radroots_sdk::transport::BlossomCancellation::default(),
            )
            .await
        {
            Ok(receipt) => {
                // The SDK verifies this exact transaction and its configuration
                // before returning the opaque remote-byte-verification receipt.
                loaded
                    .payload
                    .media_mut(&input.reference)?
                    .complete_transfer(&receipt)?;
                self.save_submission_media(&mut loaded).await?;
                self.submission_operation_status(&input.submission).await
            }
            Err(error) => {
                loaded
                    .payload
                    .media_mut(&input.reference)?
                    .fail_transfer(&error, phase1_operation_now_unix_ms()?)?;
                self.save_submission_media(&mut loaded).await?;
                Err(Phase1DraftError::Operation.into())
            }
        }
    }

    async fn save_submission_media(&self, loaded: &mut LoadedOperation) -> Result<(), E> {
        let stage = loaded.payload.stage();
        let next = loaded.head.successor(
            loaded.payload.encode()?,
            stage,
            (stage == AuthoredDraftStage::ReadyToSign).then_some(loaded.receipt.operation_id()),
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
