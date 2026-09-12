//! Stable user-action reservations over the existing authored draft journal.
//! Reservation does not authorize signing, delivery or strict publication.

mod capture;
mod commit;
mod intent;
mod record;
mod repository;
pub use capture::{CapturedSubmission, SubmissionCaptureError};
pub use commit::{SubmissionCommitError, SubmissionReceipt};
pub use intent::{
    SUBMISSION_INTENT_MAX_BYTES, SUBMISSION_INTENT_PAYLOAD_SCHEMA, SUBMISSION_INTENT_SCHEMA_SHA256,
    SUBMISSION_INTENT_SCHEMA_VERSION,
};
#[cfg(test)]
mod fault_store;
#[cfg(test)]
mod test_support;
#[cfg(test)]
mod transaction_test_support;

use radroots_storage::{Error, authored_draft::AuthoredDraftId};
use serde::{Deserialize, Serialize};

use super::{ComposerDraft, ComposerId, ComposerPersistenceError, ComposerRevision, ComposerScope};
use crate::runtime::lifecycle::RuntimeLifecycleError;

pub use record::{
    SUBMISSION_RESERVATION_MAX_BYTES, SUBMISSION_RESERVATION_PAYLOAD_SCHEMA,
    SUBMISSION_RESERVATION_SCHEMA_SHA256, SUBMISSION_RESERVATION_SCHEMA_VERSION,
};

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum SubmissionReservationError {
    #[error("invalid submission command identity")]
    InvalidCommandId,
    #[error("submission command conflicts with its saved request")]
    IdempotencyConflict,
    #[error("submission reservation requires repair")]
    CorruptRecord,
    #[error("submission reservation schema is unsupported")]
    UnsupportedSchema,
    #[error("submission reservation receipt is unconfirmed")]
    InvalidReceipt,
    #[error("submission reservation time is unavailable")]
    ClockUnavailable,
    #[error(transparent)]
    Source(#[from] ComposerPersistenceError),
    #[error(transparent)]
    Lifecycle(#[from] RuntimeLifecycleError),
    #[error("submission reservation storage failed: {0}")]
    Storage(Error),
}

impl From<Error> for SubmissionReservationError {
    fn from(error: Error) -> Self {
        Self::Storage(error)
    }
}

/// One intentional action, independent of content, composer identity or process lifetime.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(try_from = "[u8; 16]", into = "[u8; 16]")]
pub struct SubmissionCommandId([u8; 16]);

impl SubmissionCommandId {
    pub fn generate() -> Result<Self, SubmissionReservationError> {
        Self::new(*uuid::Uuid::new_v4().as_bytes())
    }

    pub fn new(bytes: [u8; 16]) -> Result<Self, SubmissionReservationError> {
        if bytes == [0; 16] {
            return Err(SubmissionReservationError::InvalidCommandId);
        }
        Ok(Self(bytes))
    }

    pub const fn as_bytes(&self) -> &[u8; 16] {
        &self.0
    }
}

impl TryFrom<[u8; 16]> for SubmissionCommandId {
    type Error = SubmissionReservationError;
    fn try_from(bytes: [u8; 16]) -> Result<Self, Self::Error> {
        Self::new(bytes)
    }
}

impl From<SubmissionCommandId> for [u8; 16] {
    fn from(value: SubmissionCommandId) -> Self {
        value.0
    }
}

/// Names a complete immutable saved revision, never caller-supplied replacement content.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SubmissionReservationRequest {
    command_id: SubmissionCommandId,
    scope: ComposerScope,
    composer_id: ComposerId,
    expected_revision: ComposerRevision,
}

impl SubmissionReservationRequest {
    pub fn new(
        command_id: SubmissionCommandId,
        scope: ComposerScope,
        composer_id: ComposerId,
        expected_revision: ComposerRevision,
    ) -> Self {
        Self {
            command_id,
            scope,
            composer_id,
            expected_revision,
        }
    }
    pub const fn command_id(&self) -> SubmissionCommandId {
        self.command_id
    }
    pub fn scope(&self) -> &ComposerScope {
        &self.scope
    }
    pub const fn composer_id(&self) -> ComposerId {
        self.composer_id
    }
    pub const fn expected_revision(&self) -> ComposerRevision {
        self.expected_revision
    }
}

/// A committed reservation and its exact historical source; not a queued operation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SubmissionReservationReceipt {
    request: SubmissionReservationRequest,
    reservation_id: AuthoredDraftId,
    captured: ComposerDraft,
    source: radroots_storage::authored_draft_submission::AuthoredDraftSource,
    reserved_at_unix_ms: u64,
    replayed: bool,
}

impl SubmissionReservationReceipt {
    fn same_request(&self, other: &Self) -> bool {
        self.request == other.request
            && self.reservation_id == other.reservation_id
            && self.captured == other.captured
            && self.source == other.source
            && self.reserved_at_unix_ms == other.reserved_at_unix_ms
    }

    pub fn request(&self) -> &SubmissionReservationRequest {
        &self.request
    }
    pub const fn reservation_id(&self) -> AuthoredDraftId {
        self.reservation_id
    }
    pub fn captured(&self) -> &ComposerDraft {
        &self.captured
    }
    pub const fn reserved_at_unix_ms(&self) -> u64 {
        self.reserved_at_unix_ms
    }
    pub const fn is_replay(&self) -> bool {
        self.replayed
    }
}
