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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    reservation: Option<UploadReservation>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct UploadReservation {
    revision: u64,
    unix_ms: u64,
}

/// Redacted durable identity; never a credential or permission to retry.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UploadAttemptIdentity {
    pub operation_id: [u8; 16],
    pub revision: Option<u64>,
    pub expiration_unix_s: u64,
}

impl UploadAttempt {
    pub(super) fn new(plan: &Phase1UploadPlan, url: &BlobUrl) -> Result<Self, Phase1DraftError> {
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
            reservation: None,
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
            || self.reservation.as_ref().is_some_and(|value| {
                value.revision == 0
                    || value.unix_ms / 1000 < self.created_at_unix_s
                    || value.unix_ms / 1000 >= self.expiration_unix_s
            })
            || !lifetime.is_some_and(|value| {
                (1..=RADROOTS_BLOSSOM_AUTH_MAX_HORIZON_SECONDS).contains(&value)
            })
        {
            return Err(Phase1DraftError::InvalidMedia);
        }
        Ok(())
    }

    pub(super) fn identity(&self) -> UploadAttemptIdentity {
        UploadAttemptIdentity {
            operation_id: self.operation_id,
            revision: self.reservation.as_ref().map(|value| value.revision),
            expiration_unix_s: self.expiration_unix_s,
        }
    }

    pub(super) fn conflicts_with(&self, other: &Self) -> bool {
        self.operation_id == other.operation_id || self.artifact_id == other.artifact_id
    }

    pub(super) fn reserve_at(
        &mut self,
        revision: u64,
        unix_ms: u64,
    ) -> Result<(), Phase1DraftError> {
        if self.reservation.is_some() {
            return Err(Phase1DraftError::InvalidMedia);
        }
        self.reservation = Some(UploadReservation { revision, unix_ms });
        self.validate()
    }

    pub(super) fn renewal_ready(&self, now: u64, delay: std::time::Duration, failed: bool) -> bool {
        let reserved = self.reservation.as_ref().map_or_else(
            || self.created_at_unix_s.saturating_mul(1000),
            |value| value.unix_ms,
        );
        now / 1000 > self.created_at_unix_s
            && now
                .checked_sub(reserved)
                .is_some_and(|elapsed| u128::from(elapsed) >= delay.as_millis())
            && (failed || now / 1000 >= self.expiration_unix_s)
    }
}

impl Phase1MediaPrerequisite {
    pub(in crate::runtime::product_surface) fn recovery_attempt(
        &self,
    ) -> Result<SigningOperationId, Phase1DraftError> {
        let attempt = self
            .authorization_attempt
            .as_ref()
            .ok_or(Phase1DraftError::InvalidMedia)?;
        attempt.validate()?;
        SigningOperationId::new(attempt.operation_id).map_err(|_| Phase1DraftError::InvalidMedia)
    }

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
