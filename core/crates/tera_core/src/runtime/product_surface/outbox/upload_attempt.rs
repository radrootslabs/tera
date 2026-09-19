//! Redacted association persisted before a bearer credential can be requested.

use super::*;
use radroots_blossom::authorization::{
    AuthorizationContent, RADROOTS_BLOSSOM_AUTH_MAX_HORIZON_SECONDS, ServerDomain,
};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct UploadAttempt {
    schema_version: u8,
    operation_id: [u8; 16],
    artifact_id: [u8; 16],
    content_sha256: [u8; 32],
    created_at_unix_s: u64,
    expiration_unix_s: u64,
    signing_deadline_unix_ms: u64,
}

impl UploadAttempt {
    fn new(plan: &Phase1UploadPlan, url: &BlobUrl) -> Result<Self, Phase1DraftError> {
        let invalid = |_| Phase1DraftError::InvalidMedia;
        let claim = AuthoredUploadClaim::new(
            AuthorizationContent::parse(&plan.authorization_content).map_err(invalid)?,
            ServerDomain::parse(url.host()).map_err(invalid)?,
            url.hash_path().hash(),
            plan.authorization_created_at_unix_s,
            plan.authorization_lifetime_seconds,
        )
        .map_err(invalid)?;
        SignPolicy::new(plan.signing_deadline_unix_ms, plan.cancellation.signing())
            .map_err(|_| Phase1DraftError::InvalidMedia)?;
        let value = Self {
            schema_version: 1,
            operation_id: *plan.operation_id.as_bytes(),
            artifact_id: *plan.artifact_id.as_bytes(),
            content_sha256: Sha256::digest(claim.content().as_str().as_bytes()).into(),
            created_at_unix_s: claim.created_at(),
            expiration_unix_s: claim.expiration(),
            signing_deadline_unix_ms: plan.signing_deadline_unix_ms,
        };
        value.validate()?;
        Ok(value)
    }

    pub(super) fn validate(&self) -> Result<(), Phase1DraftError> {
        let lifetime = self.expiration_unix_s.checked_sub(self.created_at_unix_s);
        if self.schema_version != 1
            || SigningOperationId::new(self.operation_id).is_err()
            || AuthoredArtifactId::new(self.artifact_id).is_err()
            || self.content_sha256 == [0; 32]
            || self.created_at_unix_s == 0
            || self.signing_deadline_unix_ms == 0
            || !lifetime.is_some_and(|value| {
                (1..=RADROOTS_BLOSSOM_AUTH_MAX_HORIZON_SECONDS).contains(&value)
            })
        {
            return Err(Phase1DraftError::InvalidMedia);
        }
        Ok(())
    }
}

impl Phase1MediaPrerequisite {
    pub(in crate::runtime::product_surface) fn reserve_upload(
        &mut self,
        plan: &Phase1UploadPlan,
        transaction: &radroots_sdk::transport::BlossomUploadTransaction,
    ) -> Result<(), Phase1DraftError> {
        // Legacy uploading rows without an association remain uncertain too.
        // A failure or a lost callback cannot itself authorize a replacement.
        let request = transaction.request();
        if self.authorization_attempt.is_some()
            || self.stage == Phase1MediaStage::Uploading
            || self.orphan.is_some()
            || self.url != transaction.expected_url().as_str()
            || self.sha256 != request.sha256().to_string()
            || self.byte_size != request.byte_size()
            || self.media_type != request.media_type().as_str()
        {
            return Err(Phase1DraftError::InvalidMedia);
        }
        let attempt = UploadAttempt::new(plan, transaction.expected_url())?;
        self.transition_requested(Phase1MediaStage::Uploading, None)?;
        self.authorization_attempt = Some(attempt);
        self.validate()
    }
}
