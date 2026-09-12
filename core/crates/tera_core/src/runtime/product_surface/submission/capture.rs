//! Strict captured input; durable operation installation belongs to the submission transaction.

use std::sync::Arc;

use radroots_event_codec::authoring::AuthoredEventPlan;
use radroots_sdk::transport::{BlossomConfigFingerprint, BlossomSlot};

use super::{
    SubmissionReservationError, SubmissionReservationReceipt, SubmissionReservationRequest,
};
use crate::{
    TeraRuntime,
    runtime::product_surface::{
        ComposerPersistenceError, Phase1AddCommand, Phase1MediaPrerequisite, Phase1QueuePolicy,
    },
};

mod form;
mod media;
#[cfg(test)]
mod tests;

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum SubmissionCaptureError {
    #[error(transparent)]
    Reservation(#[from] SubmissionReservationError),
    #[error("invalid publication input: {0}")]
    InvalidInput(&'static str),
    #[error("publication policy is unavailable")]
    PolicyUnavailable,
}

/// An owned immutable plan and its complete source binding. This is not a commit receipt.
/// It contains no secret, signer, transport handle or live form reference.
#[derive(Clone)]
pub struct CapturedSubmission {
    reservation: SubmissionReservationReceipt,
    command: Phase1AddCommand,
    plan: AuthoredEventPlan,
    media: Vec<Phase1MediaPrerequisite>,
    media_policy: Option<BlossomConfigFingerprint>,
    policy: Phase1QueuePolicy,
}

impl std::fmt::Debug for CapturedSubmission {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CapturedSubmission")
            .finish_non_exhaustive()
    }
}

impl CapturedSubmission {
    pub fn reservation(&self) -> &SubmissionReservationReceipt {
        &self.reservation
    }
    pub fn command(&self) -> &Phase1AddCommand {
        &self.command
    }
    pub fn plan(&self) -> &AuthoredEventPlan {
        &self.plan
    }
    pub fn media(&self) -> &[Phase1MediaPrerequisite] {
        &self.media
    }
    pub fn policy(&self) -> &Phase1QueuePolicy {
        &self.policy
    }
    pub fn media_policy(&self) -> Option<BlossomConfigFingerprint> {
        self.media_policy
    }

    /// Compares semantic request identity, excluding the transient replay observation.
    pub fn same_request(&self, other: &Self) -> bool {
        self.reservation.request() == other.reservation.request()
            && self.reservation.captured() == other.reservation.captured()
            && self.reservation.reserved_at_unix_ms() == other.reservation.reserved_at_unix_ms()
            && self.command == other.command
            && self.media == other.media
            && self.media_policy == other.media_policy
            && self.policy == other.policy
            && self.plan == other.plan
    }

    fn capture(
        reservation: SubmissionReservationReceipt,
        policy: Phase1QueuePolicy,
        blossom: Option<&BlossomSlot>,
        bytes: Vec<Arc<[u8]>>,
    ) -> Result<Self, SubmissionCaptureError> {
        let input = reservation.captured().form().input();
        let media_policy = if input.media.is_empty() {
            None
        } else {
            Some(
                blossom
                    .and_then(BlossomSlot::config_fingerprint)
                    .ok_or(SubmissionCaptureError::PolicyUnavailable)?,
            )
        };
        let prepared = media::prepare(&input.media, blossom, media_policy, bytes)?;
        let command = form::command(input, &prepared, reservation.reserved_at_unix_ms() / 1000)?;
        let plan = command
            .authored_plan(
                reservation.reserved_at_unix_ms() / 1000,
                hex::encode(reservation.request().scope().author().as_bytes()),
            )
            .map_err(|_| SubmissionCaptureError::InvalidInput("invalid_plan"))?;
        let media = prepared
            .iter()
            .map(|item| {
                Phase1MediaPrerequisite::new(item.input.opaque_reference.clone(), &item.descriptor)
                    .map_err(|_| SubmissionCaptureError::InvalidInput("invalid_media_reference"))
            })
            .collect::<Result<_, _>>()?;
        Ok(Self {
            reservation,
            command,
            plan,
            media,
            media_policy,
            policy,
        })
    }
}

impl TeraRuntime {
    /// Validates the reserved historical revision and freezes current target policy.
    /// Media bytes must match the captured metadata in order; no upload or signer is invoked.
    pub async fn submission_capture(
        &self,
        request: &SubmissionReservationRequest,
        media_bytes: Vec<Arc<[u8]>>,
    ) -> Result<CapturedSubmission, SubmissionCaptureError> {
        let _command = self
            .lifecycle
            .enter()
            .map_err(SubmissionReservationError::from)?;
        if self.store_public_key != Some(request.scope().author()) {
            return Err(SubmissionReservationError::Source(
                ComposerPersistenceError::ScopeMismatch,
            )
            .into());
        }
        let reservation = self.submission_reserve(request).await?;
        let policy = self
            .active_queue_policy(reservation.reserved_at_unix_ms())
            .map_err(|_| SubmissionCaptureError::PolicyUnavailable)?;
        let blossom = self
            .client
            .blossom()
            .map_err(|_| SubmissionCaptureError::PolicyUnavailable)?;
        CapturedSubmission::capture(reservation, policy, blossom, media_bytes)
    }
}
