//! Host-invoked foreground execution through the existing hardened SDK transport.

use super::*;
use crate::runtime::product_surface::{
    Phase1UploadPlan, SubmissionOperationStatus, phase1_new_operation_id,
    phase1_operation_now_unix_ms,
};

impl TeraRuntime {
    /// Uploads frozen media without exporting bearer authority to a native task.
    /// The host retains this bounded operation until its durable result returns.
    pub async fn submission_upload_media(
        &self,
        input: SubmissionMediaRequest,
    ) -> Result<SubmissionOperationStatus, E> {
        self.upload_submission_media_at(input, None, phase1_operation_now_unix_ms()?)
            .await
    }

    pub(in crate::runtime::product_surface::submission) async fn upload_submission_media_at(
        &self,
        input: SubmissionMediaRequest,
        renewal: Option<SubmissionUploadRenewal>,
        now: u64,
    ) -> Result<SubmissionOperationStatus, E> {
        let _command = self.lifecycle.enter().map_err(Phase1DraftError::from)?;
        self.validate_submission_owner(&input.submission)?;
        let id = super::super::intent::intent_id(&input.submission)?;
        let _admission = self.mutations.draft(*id.as_bytes())?;
        let (mut loaded, _) = self.load_submission_operation(&input.submission).await?;
        self.require_submission_running(&input.submission).await?;
        let transaction = self.materialize_submission_media(&loaded, &input, false)?;
        let plan =
            Phase1UploadPlan::derive(now, phase1_new_operation_id()?, phase1_new_operation_id()?)?;
        self.reserve_submission_upload(&mut loaded, &input, &transaction, &plan, renewal, now)
            .await?;
        // Commit the exact attempt before the first possibly interactive await.
        self.save_submission_media(&mut loaded).await?;
        self.require_submission_running(&input.submission).await?;
        let authorization = self
            .authorize_upload_transaction(&transaction, &plan)
            .await?;
        self.require_submission_running(&input.submission).await?;
        let blossom = self
            .client
            .blossom()
            .map_err(|_| Phase1DraftError::OperationUnavailable)?
            .ok_or(Phase1DraftError::OperationUnavailable)?;
        if blossom.config_fingerprint() != Some(transaction.config_fingerprint()) {
            return Err(E::MediaPolicyChanged);
        }
        self.require_submission_running(&input.submission).await?;
        match blossom
            .upload(
                transaction,
                authorization,
                radroots_sdk::transport::BlossomCancellation::default(),
            )
            .await
        {
            Ok(receipt) => {
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
}
