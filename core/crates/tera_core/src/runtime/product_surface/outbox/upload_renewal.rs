//! Renewal consumes a distinct bounded reservation; historical authority is retained.

use super::*;
use crate::runtime::product_surface::submission::SubmissionOperationError as E;
use radroots_sdk::transport::BlossomUploadTransaction;

impl Phase1MediaPrerequisite {
    pub fn upload_authorizations(&self) -> Vec<UploadAttemptIdentity> {
        self.previous_authorizations
            .iter()
            .chain(self.authorization_attempt.iter())
            .map(UploadAttempt::identity)
            .collect()
    }

    pub(in crate::runtime::product_surface) fn validate_authorization_lineage(
        &self,
    ) -> Result<(), Phase1DraftError> {
        // The shared transport admits at most five attempts. The transaction's
        // exact (possibly smaller) policy is checked again at renewal admission.
        if self.previous_authorizations.len() > 4
            || (!self.previous_authorizations.is_empty() && self.authorization_attempt.is_none())
        {
            return Err(Phase1DraftError::InvalidMedia);
        }
        let attempts: Vec<_> = self
            .previous_authorizations
            .iter()
            .chain(self.authorization_attempt.iter())
            .collect();
        for (index, attempt) in attempts.iter().enumerate() {
            attempt.validate()?;
            let identity = attempt.identity();
            if attempts[..index]
                .iter()
                .any(|prior| prior.conflicts_with(attempt))
            {
                return Err(Phase1DraftError::InvalidMedia);
            }
            if index > 0 {
                let prior = attempts[index - 1].identity();
                if identity.expiration_unix_s <= prior.expiration_unix_s
                    || identity.revision.is_none()
                    || prior
                        .revision
                        .zip(identity.revision)
                        .is_some_and(|(old, new)| old >= new)
                {
                    return Err(Phase1DraftError::InvalidMedia);
                }
            }
        }
        Ok(())
    }

    pub(in crate::runtime::product_surface) fn retains_authorization(
        &self,
        historical: &Self,
    ) -> bool {
        historical
            .authorization_attempt
            .as_ref()
            .is_some_and(|attempt| {
                self.authorization_attempt.as_ref() == Some(attempt)
                    || self.previous_authorizations.contains(attempt)
            })
    }

    pub(in crate::runtime::product_surface) fn retains_attempt(
        &self,
        attempt: SigningOperationId,
    ) -> bool {
        self.upload_authorizations()
            .iter()
            .any(|value| value.operation_id == *attempt.as_bytes())
    }

    pub(in crate::runtime::product_surface) fn associate_upload_revision(
        &mut self,
        revision: u64,
        now: u64,
    ) -> Result<(), Phase1DraftError> {
        self.authorization_attempt
            .as_mut()
            .ok_or(Phase1DraftError::InvalidMedia)?
            .reserve_at(revision, now)?;
        self.validate()
    }

    pub(in crate::runtime::product_surface) fn renew_upload(
        &mut self,
        plan: &Phase1UploadPlan,
        transaction: &BlossomUploadTransaction,
        revision: u64,
        now: u64,
        native_failed: bool,
    ) -> Result<(), E> {
        self.validate()?;
        let previous = self.authorization_attempt.as_ref().ok_or(E::InvalidMedia)?;
        if !matches!(
            self.stage,
            Phase1MediaStage::Uploading | Phase1MediaStage::Failed
        ) {
            return Err(E::InvalidMedia);
        }
        let completed =
            u8::try_from(self.previous_authorizations.len() + 1).map_err(|_| E::InvalidMedia)?;
        let delay = transaction
            .retry_delay_after(completed)
            .ok_or(E::UploadAttemptsExhausted)?;
        if !previous.renewal_ready(
            now,
            delay,
            native_failed || self.stage == Phase1MediaStage::Failed,
        ) {
            return Err(E::UploadRenewalPending);
        }
        let request = transaction.request();
        if self.url != transaction.expected_url().as_str()
            || self.sha256 != request.sha256().to_string()
            || self.byte_size != request.byte_size()
            || self.media_type != request.media_type().as_str()
        {
            return Err(E::InvalidMedia);
        }
        let mut next = self.clone();
        next.previous_authorizations.push(previous.clone());
        let mut attempt = UploadAttempt::new(plan, transaction.expected_url())?;
        attempt.reserve_at(revision, now)?;
        next.authorization_attempt = Some(attempt);
        next.stage = Phase1MediaStage::Uploading;
        next.failure_code = None;
        // The old failure/orphan facts remain in the exact historical journal.
        next.orphan = None;
        next.validate()?;
        *self = next;
        Ok(())
    }
}
