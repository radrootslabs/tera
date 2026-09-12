//! Existing opaque native upload authorization, shared after caller-owned validation.

use super::*;

impl TeraRuntime {
    pub(in crate::runtime::product_surface) async fn authorize_native_upload(
        &self,
        transaction: &radroots_sdk::transport::BlossomUploadTransaction,
        plan: &Phase1UploadPlan,
    ) -> Result<Phase1NativeUploadJob, Phase1DraftError> {
        let blossom = self
            .client
            .blossom()
            .map_err(|_| Phase1DraftError::OperationUnavailable)?
            .ok_or(Phase1DraftError::OperationUnavailable)?;
        let remote_url = transaction.expected_url().as_str().to_owned();
        let content = radroots_blossom::authorization::AuthorizationContent::parse(
            &plan.authorization_content,
        )
        .map_err(|_| Phase1DraftError::InvalidMedia)?;
        let claim = blossom
            .authored_upload_claim(
                transaction,
                content,
                plan.authorization_created_at_unix_s,
                plan.authorization_lifetime_seconds,
            )
            .map_err(|_| Phase1DraftError::Operation)?;
        let authorization = self
            .phase1_authorize_blossom_upload(
                *plan.operation_id.as_bytes(),
                *plan.artifact_id.as_bytes(),
                claim,
                plan.signing_deadline_unix_ms,
                plan.cancellation,
            )
            .await?;
        Ok(Phase1NativeUploadJob {
            operation_id: *plan.operation_id.as_bytes(),
            remote_url,
            authorization_header: authorization.into_string(),
            expected_sha256: transaction.request().sha256().to_string(),
            media_type: transaction.request().media_type().as_str().to_owned(),
            byte_size: transaction.request().byte_size(),
        })
    }
}
