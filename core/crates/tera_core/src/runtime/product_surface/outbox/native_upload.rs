//! Existing opaque native upload authorization, shared after caller-owned validation.

use super::*;

/// Immutable Rust-derived policy for one upload attempt.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Phase1UploadPlan {
    pub authorization_content: String,
    pub authorization_created_at_unix_s: u64,
    pub authorization_lifetime_seconds: u64,
    pub operation_id: SigningOperationId,
    pub artifact_id: AuthoredArtifactId,
    pub signing_deadline_unix_ms: u64,
    pub cancellation: Phase1CancellationPolicy,
    pub updated_at_unix_ms: u64,
}

impl Phase1UploadPlan {
    pub(in crate::runtime::product_surface) fn derive(
        now_unix_ms: u64,
        operation_id: [u8; 16],
        artifact_id: [u8; 16],
    ) -> Result<Self, Phase1DraftError> {
        let now_unix_s = now_unix_ms / 1_000;
        if now_unix_s == 0 {
            return Err(Phase1DraftError::ClockUnavailable);
        }
        Ok(Self {
            authorization_content: BLOSSOM_AUTHORIZATION_CONTENT.to_owned(),
            authorization_created_at_unix_s: now_unix_s
                .saturating_sub(BLOSSOM_AUTHORIZATION_BACKDATE_SECONDS),
            authorization_lifetime_seconds: BLOSSOM_AUTHORIZATION_LIFETIME_SECONDS,
            operation_id: SigningOperationId::new(operation_id)
                .map_err(|_| Phase1DraftError::InvalidDraft)?,
            artifact_id: AuthoredArtifactId::new(artifact_id)
                .map_err(|_| Phase1DraftError::InvalidDraft)?,
            signing_deadline_unix_ms: now_unix_ms
                .checked_add(BLOSSOM_SIGNING_TIMEOUT_MS)
                .ok_or(Phase1DraftError::DeadlineOverflow)?,
            cancellation: Phase1CancellationPolicy::LocalCooperative,
            updated_at_unix_ms: now_unix_ms,
        })
    }
}

impl TeraRuntime {
    pub(in crate::runtime::product_surface) async fn authorize_native_upload(
        &self,
        transaction: &radroots_sdk::transport::BlossomUploadTransaction,
        plan: &Phase1UploadPlan,
    ) -> Result<Phase1NativeUploadJob, Phase1DraftError> {
        let authorization = self.authorize_upload_transaction(transaction, plan).await?;
        Ok(Phase1NativeUploadJob {
            operation_id: *plan.operation_id.as_bytes(),
            remote_url: transaction.expected_url().as_str().to_owned(),
            upload_url: transaction.expected_url().upload_url(),
            authorization_header: authorization.into_string(),
            expected_sha256: transaction.request().sha256().to_string(),
            media_type: transaction.request().media_type().as_str().to_owned(),
            byte_size: transaction.request().byte_size(),
        })
    }

    pub(in crate::runtime::product_surface) async fn authorize_upload_transaction(
        &self,
        transaction: &radroots_sdk::transport::BlossomUploadTransaction,
        plan: &Phase1UploadPlan,
    ) -> Result<radroots_sdk::signing::AuthorizationHeader, Phase1DraftError> {
        let blossom = self
            .client
            .blossom()
            .map_err(|_| Phase1DraftError::OperationUnavailable)?
            .ok_or(Phase1DraftError::OperationUnavailable)?;
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
        self.phase1_authorize_blossom_upload(
            *plan.operation_id.as_bytes(),
            *plan.artifact_id.as_bytes(),
            claim,
            plan.signing_deadline_unix_ms,
            plan.cancellation,
        )
        .await
    }
}
